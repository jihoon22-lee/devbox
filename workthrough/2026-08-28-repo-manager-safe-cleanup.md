# Repo Manager safe branch · worktree cleanup (#364)

## Overview

Issue #364 adds a preview-first cleanup flow to Repo Manager. Local branches are
shown with bounded merged, gone-upstream, and 90-day inactivity rationale;
linked or detached worktrees are shown with explicit safety blockers. Only
targets selected from the latest preview and confirmation can reach the native
mutation boundary.

The implementation is a PR candidate on a dedicated worktree based on
`origin/main` commit `952d2a7604eb2739c8e88eb1c3f21a597ae931eb`. No commit,
push, or pull request was created.

## Context

Before this work, Repo Manager exposed only a read-only `worktree_clean`
command and a per-row “remove 확인” check. There was no bounded rationale
surface, stale-preview guard, or safe native removal path. The issue explicitly
excludes force delete, reset, clean, worktree prune, and destructive recovery.

## Changes Made

### 1. Bounded cleanup core

File: `apps/repo-manager/src-tauri/src/core/cleanup.rs`

- Added strict parsers for fixed `for-each-ref`, merged-ref,
  `worktree list --porcelain -z`, and porcelain status output.
- Added output, record, branch, worktree, ref, path, and selection bounds.
- Rejects malformed NUL records, duplicate records, unsafe ref/path metadata,
  device namespace paths, invalid object IDs, and oversized output with one
  fixed error.
- Classifies merged, gone-upstream, and inactive branch candidates while
  blocking current, `main`, and checked-out branches.
- Classifies primary, current, bare, prunable, locked, dirty, untracked,
  ignored, and unavailable worktrees with stable rationale/block IDs.
- Generates an opaque preview revision without exposing path or branch values.

### 2. Native command and mutation boundary

Files:

- `apps/repo-manager/src-tauri/src/commands.rs`
- `apps/repo-manager/src-tauri/src/core/mod.rs`
- `apps/repo-manager/src-tauri/src/lib.rs`

- Added `repo_cleanup_preview`, `repo_cleanup`, and `repo_cleanup_cancel`. Preview now
  accepts its own bounded operation ID, so unmount/repository replacement can
  cancel an in-flight metadata read as well as a mutation.
- Uses fixed argument vectors and bounded native execution. The only cleanup
  mutations are `git branch --delete -- <branch>` and
  `git worktree remove -- <path>`.
- Revalidates repository/common Git directory identity, worktree filesystem
  identity, preview revision, worktree status, lock/prunable state, and
  registration immediately before removal. Branches are also re-read for
  object ID, upstream, current/checked-out state, merged/stale classification,
  and current primary HEAD; worktrees are re-read for canonical path, identity,
  HEAD, branch, primary/bare/locked/prunable registration, and clean status.
- Shares the canonical common-directory single-flight registry with local and
  remote Git operations. Cancellation is addressed only by an opaque operation
  ID and never reopens the repository path.
- Cleanup reads use a fixed command allow-list at the final argv boundary,
  cooperative cancellation, bounded stdout, and one total observation or
  revalidation deadline. The selected mutation batch additionally has a
  120-second total budget and each child receives the remaining bounded
  timeout, so a large approved selection cannot occupy the repository lock
  indefinitely. The allow-list admits only the fixed branch/worktree/status
  reads and the validated hexadecimal `--merged=<object>` query, plus
  non-force branch delete/worktree remove mutations.
- Blocked selections return per-item results without mutation; stale or
  exchanged state returns a fixed state-change error. Empty, duplicate, or
  over-bound selections are rejected before native execution.
- An unborn HEAD is accepted only when the parsed primary worktree reports
  Git's all-zero HEAD; other `rev-parse` failures remain fail-closed.
- Selection validators reuse Git ref/path validation and allow Windows verbatim
  drive/UNC long paths while rejecting physical-device and NT object-manager
  namespaces. Merged-ref parsing has an explicit branch-count cap.

### 3. Frontend preview and result UX

Files:

- `apps/repo-manager/src/api.ts`
- `apps/repo-manager/src/components/CleanupPanel.tsx`
- `apps/repo-manager/src/components/CleanupPanel.test.tsx`
- `apps/repo-manager/src/App.tsx`
- `apps/repo-manager/src/App.css`
- `apps/repo-manager/src/App.test.tsx`
- `apps/repo-manager/src/App.applink.test.tsx`

- Added typed preview/result APIs and browser-safe fixtures.
- Added explicit “정리 후보 검사”, candidate rationale, disabled blocker
  rows, selection counts, confirmation snapshot, cancellation, and per-item
  result display.
- Busy, sequence, repository identity, selection, and unmount guards prevent
  duplicate actions and stale responses. Native errors are reduced to the
  fixed cleanup vocabulary and never echo raw diagnostics.
- Explicit cancel invalidates the panel request sequence before calling the
  native cancel command. A native success/error that races with cancellation
  is therefore discarded; cleanup, remote, and local panels clear their
  snapshot/selection and require a fresh read before showing another action.
- The confirmation dialog lists every selected branch and worktree target so
  the approval is tied to an exact, human-readable set. A failed, cancelled,
  stale, state-change, or revision-mismatched mutation result clears the
  preview and selection and requires a fresh candidate scan; long target lists
  remain scrollable and focus-safe.
- Removed the old row-level remove-check action from the main flow while
  retaining the read-only command/API for compatibility.
- The selected/current linked worktree is also blocked, preventing a cleanup
  command from removing its own working directory.
- Cleanup batches are deliberately not presented as atomic transactions:
  branch/worktree deletion cannot be safely rolled back after a prior item has
  succeeded. A partial or uncertain result is closed with a fixed error and a
  fresh preview requirement, without attempting force recovery.

### 4. Fixtures and documentation

Added native fixtures for:

- merged/gone/inactive branch rationale;
- main/current/checked-out/dirty/untracked/ignored/locked/prunable and
  unavailable blockers;
- malformed, duplicate, oversized, traversal, and device-style metadata;
- unborn HEAD handling, fixed non-destructive argv, blocked-selection status
  preservation, clean linked-worktree removal, explicit branch deletion, and
  stale revision rejection.

Updated:

- `apps/repo-manager/README.md`
- `docs/superpowers/specs/2026-08-14-repo-manager-design.md`
- `docs/architecture.md`
- `docs/roadmap.md`

Also hardened the shared `crates/git` child environment: repository-selection
overrides and Git's `GIT_CONFIG_PARAMETERS`/`GIT_CONFIG_COUNT` plus conventional
`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n` pairs are removed before every bounded
child. Normal user Git config and credential/SSH/askpass variables remain
available, but repository redirection through inherited environment config is
not.

As part of the grouped Git safety audit, remote operation-marker lookup now
accepts only the fixed marker names used by the state machine and rejects
traversal or mismatched final path components before checking the marker. This
keeps the read-only remote preflight path bounded even if a future caller tries
to pass a dynamic marker.

### 5. Final integrated audit remediation

The final #364 review also covered the shared runner and the complete mutation
deadline, not only the cleanup command's direct argv:

- `crates/git` now permits line breaks, carriage returns, and tabs only in the
  explicit `--message`/`-m` commit-message slot of a mutating argv. A future
  caller cannot accidentally use the broad commit-message allowance for a
  path, remote, hook, author, or other control-bearing argument.
- The bounded stdout reader drains bytes already available after Git exits, but
  uses a 100ms finite post-exit drain window. Unix uses a nonblocking descriptor;
  Windows polls synchronous pipe availability and treats a normal broken-pipe
  close as EOF. This preserves ordinary Git output while preventing a
  descendant that escaped process-tree ownership and retained the pipe from
  making `reader.join()` unbounded.
- Cleanup's 120-second mutation budget is now propagated through repository
  identity revalidation, branch/worktree metadata reads, final context checks,
  and each child mutation. Every operation receives only the remaining parent
  budget, and cancellation is checked at each boundary.
- Cleanup preview/revalidation now derives the merge base from the selected
  repository worktree, not blindly from `worktree list`'s primary-first row.
  This keeps a linked-worktree invocation from presenting branches merged only
  into another worktree as candidates. The opaque revision also binds the
  common Git-directory identity, so an equivalent-looking `.git` replacement
  cannot reuse an earlier approval; selected-worktree identity is checked once
  more immediately before the remove child.
- The shared mutation runner only allows line-break controls in a message
  argument that follows an actual `commit` command. Cleanup/remote/future
  mutation arguments cannot opt into the commit-message exception merely by
  spelling `--message=`.
- The frontend state-change assertion was corrected to match the complete
  status sentence while still checking the required fresh-preview guidance;
  this avoids coupling a test to an implementation detail of one status node.

The documents describe the public DTOs, fixed Git argv, identity/status
revalidation, fixed errors, privacy boundary, and the deliberate absence of
force/reset/clean/prune behavior.

### 6. Focused follow-up remediation

This follow-up keeps the existing #364 candidate intact and addresses the
remaining small correctness gaps identified during the final pre-PR audit:

- `apps/repo-manager/src-tauri/src/core/cleanup.rs` now recognizes an unborn
  worktree HEAD by its repository object format. Both 40-character SHA-1 and
  64-character SHA-256 all-zero object IDs normalize to `head: None`; the
  parser no longer compares against a SHA-1-only constant. A focused fixture
  covers both widths.
- `apps/repo-manager/src-tauri/src/commands.rs` routes cleanup repository
  context revalidation through the same `run_bounded_with_cancel` runner used
  by the other cleanup reads. The remaining timeout calculated from the
  caller's absolute deadline and the operation's cancellation token are
  passed through the Git common-directory query. Cancellation is translated
  to the fixed cleanup cancellation error rather than a generic state error.
- The synchronous `canonicalize`/`filesystem_identity` portions of cleanup
  context validation now have operation-boundary checks before and after each
  filesystem step. Cleanup's initial repository validation and subsequent
  context revalidation both use these checks, and the caller checks the same
  deadline again after the helper returns. The whole command remains on the
  existing Tauri bounded `spawn_blocking` worker; no nested or detached thread
  is introduced. A native filesystem syscall cannot be forcibly interrupted
  after entry, so the contract is to observe cancellation/deadline immediately
  before/after the syscall and prevent any later Git child or mutation when it
  returns late.
- `commands.rs` adds focused tests for fixed cancellation mapping at the
  cleanup runner boundary and for cancellation/expired-deadline behavior at
  context revalidation boundaries.

The requested scope deliberately does not alter the selected worktree
remove-path TOCTOU boundary or the shared `crates/git` Windows process-tree /
Unix descendant implementation; those items remain with the parent agent.

### 7. Parent process-tree remediation

The final parent review closed the actionable Windows process-tree ownership
gap in the shared bounded Git runner. `crates/git` now creates every bounded
Windows Git child with `CREATE_SUSPENDED | CREATE_NO_WINDOW`, assigns the root
process to the kill-on-close Job Object, resolves the exact child PID's sole
suspended primary thread, and resumes it only after assignment. Any missing or
ambiguous thread, unexpected suspend count, Job assignment failure, or resume
failure terminates and reaps the child without letting Git user code execute
outside the Job. The existing Windows tests exercise this path through the
ordinary bounded runner; a dedicated GNU Windows compile check covers the
conditional API surface before GitHub's MSVC job.

The selected-worktree path replacement interval cannot be made atomic while
delegating registered-worktree deletion to the Git CLI: retaining a directory
handle that denies rename would also deny Git's own removal, while an
allow-delete handle does not prevent substitution. The implementation keeps
the final identity/status/registration observation immediately before the
no-force `git worktree remove -- <exact path>` child and documents this as a
same-user concurrent filesystem mutation boundary rather than claiming an
OS-level atomic delete guarantee. Reimplementing Git's worktree administration
and recursive deletion is intentionally rejected as a less reliable safety
boundary.

## Code Examples

```rust
// apps/repo-manager/src-tauri/src/commands.rs
if observation.preview.revision != request.preview_revision {
    return Err(GIT_CLEANUP_STATE_CHANGED.to_string());
}

let args = git_cleanup_delete_branch_args(&name);
cleanup_branch_still_safe(&context, &expected, expected_head.as_deref(), cancellation)?;
let timeout = cleanup_remaining_mutation_timeout(mutation_deadline, cancellation)?;
run_git_cleanup_mutation_with_cancel(&args, &context.worktree, cancellation, timeout)?;
```

```tsx
// apps/repo-manager/src/components/CleanupPanel.tsx
const next = await repoCleanup(
  repo.path,
  pending.branches,
  pending.worktrees,
  pending.revision,
  operationId,
);
```

The UI confirmation is built from the same pending selection sent to native:

```tsx
summary={[
  "정리 대상:",
  ...pending.branches.map((name) => `branch ${name}`),
  ...pending.worktrees.map((path) => `worktree ${path}`),
]}
```

## Verification Results

Focused verification completed with the dedicated Linux-native target
`/home/jihoon/.cache/targets/repo-364` and single-job execution:

```text
CARGO_TARGET_DIR=.../repo-364 CARGO_BUILD_JOBS=1 cargo check -p repo-manager -j1                  PASS
CARGO_TARGET_DIR=.../repo-364 CARGO_BUILD_JOBS=1 cargo test -p repo-manager --lib --tests -j1     PASS (75 tests)
CARGO_TARGET_DIR=.../repo-364 CARGO_BUILD_JOBS=1 cargo test -p git --lib -j1                     PASS (14 tests)
CARGO_TARGET_DIR=.../repo-364 CARGO_BUILD_JOBS=1 cargo clippy -p repo-manager --lib --all-targets PASS (-D warnings)
CARGO_TARGET_DIR=.../repo-364 CARGO_BUILD_JOBS=1 cargo clippy -p git --all-targets                PASS (-D warnings)
cargo fmt --all -- --check                                                                      PASS
git diff --check                                                                                 PASS
```

Frontend dependencies were installed only for the filtered Repo Manager
workspace (`pnpm install --filter repo-manager... --frozen-lockfile
--ignore-scripts --child-concurrency=1 --network-concurrency=2`). The focused
cleanup suite and package build then passed:

```text
pnpm --dir apps/repo-manager exec vitest run src/components/CleanupPanel.test.tsx \
  --maxWorkers=1 --no-file-parallelism --reporter=dot                             PASS (8 tests)
pnpm --dir apps/repo-manager build                                                 PASS (tsc + Vite, 50 modules)
```

The first all-file Vitest run exercised 74 tests and reached 73 passes with one
test-only exact-text matcher failure. The rendered UI correctly contained the
required guidance as part of its complete status sentence; after changing the
matcher to an anchored substring/regex assertion, the entire affected
CleanupPanel file passed. The complete all-file suite was not repeated in this
worktree because its jsdom environment startup is serial and took over six
minutes; the parent CI gate remains authoritative for the workspace-wide run.

Windows packaged W3 smoke and CI remain release-gate work for the parent agent.

The parent review verified the complete changed Rust boundary in the retained
dedicated cache after applying the follow-up and Windows process-tree fix:

```text
cargo test -p git -j1                                  pass (14 tests)
cargo test -p repo-manager -j1                         pass (78 tests)
cargo check -p repo-manager -j1                        pass
cargo clippy -p repo-manager --all-targets -j1 -- -D warnings pass
cargo clippy -p git --all-targets -j1 -- -D warnings  pass
cargo check -p git --target x86_64-pc-windows-gnu -j1 pass
cargo fmt --all -- --check                             pass
git diff --check                                      pass
```

The prior focused Repo Manager frontend suite and production build remain
valid because this follow-up changes only native cleanup/process supervision
and documentation. GitHub CI and Windows packaged W3 smoke remain the final
platform gates.

## Handoff

- Base: `origin/main` / `952d2a7604eb2739c8e88eb1c3f21a597ae931eb`
- Branch: `feat/repo-manager/advanced-workflow`
- Worktree: `/mnt/e/projects/devbox-worktrees/repo-manager-advanced-workflow`
- Worktree remains dirty by design with the candidate changes and is not to be
  removed until the parent agent reviews/merges the work.
- Remaining risk: a filesystem path can still be swapped in the narrow interval
  after final identity observation and before Git resolves `worktree remove`;
  Git's own registration checks and no-force command are the final boundary,
  but an OS-level open-handle/no-replace remove primitive is outside this
  issue. On Unix, a malicious descendant that explicitly escapes its process
  group can survive process-tree termination, although the bounded reader now
  stops after the finite drain grace. Windows suspended Job assignment closes
  the pre-assignment escape window; packaged path/identity/process-tree/
  credential-helper/UI behavior still requires W3 smoke/CI. This review also preserves the limitation that
  synchronous filesystem metadata calls cannot be forcibly interrupted while
  resolving a hostile/unavailable network path. This worktree intentionally
  did not run the full workspace gate.
