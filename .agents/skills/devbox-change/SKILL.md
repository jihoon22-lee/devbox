---
name: devbox-change
description: Implement or maintain Devbox changes with milestone-aware PR grouping, affected verification, and concise evidence. Use for Devbox implementation, fixes, refactoring, or repository tooling changes.
---

# Devbox change

Read the repository [AGENTS.md](../../../AGENTS.md) and relevant sections of
[CONVENTIONS.md](../../../CONVENTIONS.md). Resolve these links from this skill directory.

## Establish the change boundary

- Identify the requested user behavior, relevant WP/requirement IDs, prerequisites,
  and acceptance criteria. Read current GitHub issue bodies using the available
  GitHub connection or `gh`; use local specifications for versioned contracts.
- For v0.8, follow the B01–B09 grouping in CONVENTIONS §8. Keep implementation,
  importers, regression fixtures, and documentation for one bundle in one PR.
  Explicitly requested preparation work can stand alone without completing a WP.
- Check the existing worktree, branch, and uncommitted changes. Use a dedicated
  branch/worktree when needed; preserve user-owned changes. Reuse existing domain
  code and actual consumers before introducing shared abstractions.

## Implement and verify

- Read only the affected app/module guidance and contracts. Keep mechanical moves
  separate from semantic changes in commits, with a single owner for shared edits.
- Before each commit, inspect the diff against the plan and use only the minimum
  syntax/type checks needed for that change. Documentation needs content/link/diff
  review, not compiler or test runs. Do not routinely chain tests, Clippy, builds
  and affected verification after small edits or before every commit.
- Prepare regression fixtures during implementation. Run detailed verification
  when all planned implementation, importers and fixtures for the PR bundle are
  complete. An earlier focused run needs a concrete defect or design uncertainty
  that cannot be resolved without reproducing it.
- At PR completion, map acceptance requirements to checks and run
  `pnpm verify:affected` from the root. Do not separately pre-run tests, typechecks,
  builds or lint already included in it. Add only acceptance checks it does not
  cover, including required Windows/WSL execution. Honor `all` if selected; use
  `verify:all` for CONVENTIONS §5 cases. A Windows compile is not execution evidence.
- After a failure or subsequent code change, rerun the failed/affected checks.
  Preserve passing evidence where no related change or new risk invalidates it.
  Creating a commit, updating docs or resuming work is not a reason to rerun checks.
- Keep local verification within the shared resource budget and worktree lock in
  [verification operations](../../../docs/verification.md). One full run satisfies
  both all-scope affected verification and the explicit full audit; do not duplicate it.
- Review failures and fix those attributable to the change. Record unrelated or
  environment failures accurately; never substitute a skipped check for a pass.

## Leave reviewable evidence

- Update one `workthrough/YYYY-MM-DD-scope.md` per PR bundle with the problem,
  important decisions, affected paths, actual checks/results, and remaining limits.
  Link fixtures and CI runs instead of copying full diffs or successful logs.
- In the PR, include issue mapping, grouping rationale, behavior/data/authority
  changes, validation, and rollback limits when relevant. Close only fully satisfied
  issues; use `Refs` for partial work and never auto-close #541/#542 from an implementation PR.
- Follow existing authorization for GitHub mutations. When merge is in scope,
  wait for CI on the final PR changes and complete CONVENTIONS §8 cleanup.
- Before a context handoff, preserve scope, WP IDs, worktree/branch, decisions,
  verification, and the next action. Recheck live Git/CI state on resume.

## Preserve running local services

Never provision/start/stop Docker or mutate containers, networks, firewall rules,
routes or shared host services for a local test. A disposable WSL distro, unique
socket/data-root or container name is not network isolation. Run network-changing
acceptance only on a disposable hosted runner or a separately verified independent
VM. Do not spoof CI environment variables, use private script copies or weaken the
local execution guard. Without that environment, retain an unrun acceptance item
and continue implementation. Do not restart existing services or restore firewall
state without an explicit recovery request and verified restoration evidence.
